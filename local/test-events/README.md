# Test Events

These are sample Lambda event payloads for local testing with `cargo lambda invoke`.

## Setup

Copy template files to create your local test events:

```bash
cd local/test-events
for f in *.template; do cp "$f" "${f%.template}"; done
sed -i '' 's/<ACCOUNT_ID>/YOUR_ACCOUNT_ID/g' *.json
```

Or manually:
```bash
cp search.json.template search.json
cp metadata.json.template metadata.json
cp compare.json.template compare.json
cp ingest.json.template ingest.json
# Edit ingest.json to replace <ACCOUNT_ID>
```

## Files

- `search.json` - Test hybrid search Lambda
- `metadata.json` - Test document metadata Lambda  
- `compare.json` - Test document comparison Lambda
- `ingest.json` - Test S3 ingestion Lambda

**Note:** The `.json` files are gitignored to prevent committing account-specific data.
