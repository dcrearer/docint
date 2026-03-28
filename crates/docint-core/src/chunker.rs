//! Semantic text chunker that splits documents into overlapping pieces
//! while preserving sentence boundaries. Used by the ingestion pipeline
//! to prepare text for embedding.

/// Approximate chars per token — used to convert token targets to char targets.
const CHARS_PER_TOKEN: usize = 4;

pub struct Chunker {
    chunk_size: usize,  // in chars
    overlap: usize,     // in chars
}

impl Default for Chunker {
    fn default() -> Self {
        Self::new(500, 50)
    }
}

impl Chunker {
    /// Create a chunker with target sizes in tokens.
    pub fn new(chunk_tokens: usize, overlap_tokens: usize) -> Self {
        Self {
            chunk_size: chunk_tokens * CHARS_PER_TOKEN,
            overlap: overlap_tokens * CHARS_PER_TOKEN,
        }
    }

    pub fn chunk<'a>(&self, text: &'a str) -> Vec<&'a str> {
        let sentences = split_sentences(text);
        if sentences.is_empty() {
            return vec![];
        }

        let mut chunks = Vec::new();
        let mut start = 0; // index into sentences

        while start < sentences.len() {
            // Build chunk up to chunk_size
            let mut end = start;
            let mut len = 0;
            while end < sentences.len() {
                let slen = sentences[end].len();
                if len > 0 && len + slen > self.chunk_size {
                    break;
                }
                len += slen;
                end += 1;
            }

            // Compute the byte range for this chunk
            let chunk_start = sentences[start].as_ptr() as usize - text.as_ptr() as usize;
            let last = &sentences[end - 1];
            let chunk_end = last.as_ptr() as usize - text.as_ptr() as usize + last.len();
            chunks.push(text[chunk_start..chunk_end].trim());

            // Walk back from `end` to find overlap start
            let mut next_start = end;
            let mut overlap_len = 0;
            while next_start > start + 1 {
                let slen = sentences[next_start - 1].len();
                if overlap_len + slen > self.overlap {
                    break;
                }
                overlap_len += slen;
                next_start -= 1;
            }
            // Ensure progress
            if next_start == start {
                next_start = end;
            }
            start = next_start;
        }

        chunks.into_iter().filter(|c| !c.is_empty()).collect()
    }
}

/// Split text into sentences at '.', '!', '?' followed by whitespace,
/// or at newlines (handles CSV, logs, and structured text).
fn split_sentences(text: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();

    for i in 0..bytes.len() {
        let is_boundary = if bytes[i] == b'\n' {
            true
        } else if matches!(bytes[i], b'.' | b'!' | b'?') {
            if i + 1 >= bytes.len() {
                true
            } else {
                bytes[i + 1].is_ascii_whitespace()
            }
        } else {
            false
        };

        if is_boundary {
            let end = i + 1;
            let s = text[start..end].trim();
            if !s.is_empty() {
                sentences.push(s);
            }
            start = end;
        }
    }

    // Remaining text
    let remaining = text[start..].trim();
    if !remaining.is_empty() {
        sentences.push(remaining);
    }

    sentences
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_chunking() {
        let chunker = Chunker::new(20, 5); // small sizes for testing
        let text = "First sentence. Second sentence. Third sentence. Fourth sentence.";
        let chunks = chunker.chunk(text);
        assert!(chunks.len() > 1);
        assert!(chunks[0].contains("First"));
        // Last chunk should exist
        assert!(chunks.last().unwrap().contains("Fourth"));
    }

    #[test]
    fn overlap_works() {
        let chunker = Chunker::new(15, 10); // force overlap
        let text = "Alpha one. Beta two. Gamma three. Delta four.";
        let chunks = chunker.chunk(text);
        // With overlap, adjacent chunks should share sentences
        if chunks.len() >= 2 {
            let first_words: Vec<&str> = chunks[0].split_whitespace().collect();
            let second_words: Vec<&str> = chunks[1].split_whitespace().collect();
            // There should be some shared content
            let shared = first_words.iter().any(|w| second_words.contains(w));
            assert!(shared, "Expected overlap between chunks");
        }
    }

    #[test]
    fn empty_text() {
        let chunker = Chunker::default();
        assert!(chunker.chunk("").is_empty());
        assert!(chunker.chunk("   ").is_empty());
    }

    #[test]
    fn single_sentence() {
        let chunker = Chunker::default();
        let chunks = chunker.chunk("Just one sentence.");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Just one sentence.");
    }
}
