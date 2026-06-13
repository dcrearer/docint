"""Unit tests for agent.py - TenantInjectorMCPClient and core logic."""
import pytest
from unittest.mock import AsyncMock, MagicMock, patch
from agent import TenantInjectorMCPClient


class TestTenantInjectorMCPClient:
    """Test suite for TenantInjectorMCPClient wrapper."""

    @pytest.mark.asyncio
    async def test_injects_tenant_id_into_tool_calls(self):
        """Verify tenant_id is automatically injected into all tool calls."""
        # Arrange
        mock_mcp = AsyncMock()
        mock_mcp.return_value = {"results": []}
        wrapper = TenantInjectorMCPClient("test-tenant-123", mock_mcp)

        # Act
        await wrapper("get-document-metadata", {"limit": 5})

        # Assert
        mock_mcp.assert_called_once_with(
            "get-document-metadata",
            {"limit": 5, "tenant_id": "test-tenant-123"}
        )

    @pytest.mark.asyncio
    async def test_preserves_existing_parameters(self):
        """Verify existing parameters are preserved when tenant_id is injected."""
        # Arrange
        mock_mcp = AsyncMock()
        mock_mcp.return_value = {"chunks": []}
        wrapper = TenantInjectorMCPClient("tenant-456", mock_mcp)

        # Act
        await wrapper("search-documents", {"query": "rust", "limit": 10})

        # Assert
        mock_mcp.assert_called_once_with(
            "search-documents",
            {"query": "rust", "limit": 10, "tenant_id": "tenant-456"}
        )

    @pytest.mark.asyncio
    async def test_overwrites_tenant_id_if_present(self):
        """Verify injected tenant_id overwrites any user-provided tenant_id (defense in depth)."""
        # Arrange
        mock_mcp = AsyncMock()
        mock_mcp.return_value = {}
        wrapper = TenantInjectorMCPClient("correct-tenant", mock_mcp)

        # Act - simulate malicious attempt to pass different tenant_id
        await wrapper("get-document-metadata", {"tenant_id": "evil-tenant", "limit": 5})

        # Assert - injected tenant_id should win
        mock_mcp.assert_called_once_with(
            "get-document-metadata",
            {"tenant_id": "correct-tenant", "limit": 5}
        )

    @pytest.mark.asyncio
    async def test_returns_mcp_client_result(self):
        """Verify wrapper returns the underlying MCP client's result."""
        # Arrange
        expected_result = {"documents": [{"id": "doc1", "title": "Test"}]}
        mock_mcp = AsyncMock()
        mock_mcp.return_value = expected_result
        wrapper = TenantInjectorMCPClient("tenant-789", mock_mcp)

        # Act
        result = await wrapper("get-document-metadata", {})

        # Assert
        assert result == expected_result

    @pytest.mark.asyncio
    async def test_propagates_mcp_client_exceptions(self):
        """Verify exceptions from MCP client are propagated."""
        # Arrange
        mock_mcp = AsyncMock()
        mock_mcp.side_effect = RuntimeError("Gateway timeout")
        wrapper = TenantInjectorMCPClient("tenant-999", mock_mcp)

        # Act & Assert
        with pytest.raises(RuntimeError, match="Gateway timeout"):
            await wrapper("search-documents", {"query": "test"})

    def test_delegates_attributes_to_wrapped_client(self):
        """Verify __getattr__ delegates non-callable attributes to wrapped client."""
        # Arrange
        mock_mcp = MagicMock()
        mock_mcp.some_property = "test_value"
        mock_mcp.some_method = MagicMock(return_value="method_result")
        wrapper = TenantInjectorMCPClient("tenant-abc", mock_mcp)

        # Act & Assert
        assert wrapper.some_property == "test_value"
        assert wrapper.some_method() == "method_result"

    @pytest.mark.asyncio
    async def test_handles_empty_tool_input(self):
        """Verify wrapper handles empty tool input (no parameters besides tenant_id)."""
        # Arrange
        mock_mcp = AsyncMock()
        mock_mcp.return_value = {"documents": []}
        wrapper = TenantInjectorMCPClient("tenant-empty", mock_mcp)

        # Act
        await wrapper("get-document-metadata", {})

        # Assert
        mock_mcp.assert_called_once_with(
            "get-document-metadata",
            {"tenant_id": "tenant-empty"}
        )

    @pytest.mark.asyncio
    async def test_handles_multiple_calls_with_same_tenant(self):
        """Verify wrapper maintains tenant_id across multiple calls."""
        # Arrange
        mock_mcp = AsyncMock()
        mock_mcp.return_value = {}
        wrapper = TenantInjectorMCPClient("persistent-tenant", mock_mcp)

        # Act
        await wrapper("search-documents", {"query": "test1"})
        await wrapper("get-document-metadata", {"limit": 5})
        await wrapper("compare-documents", {"query": "test2", "document_id_a": "a", "document_id_b": "b"})

        # Assert
        assert mock_mcp.call_count == 3
        for call in mock_mcp.call_args_list:
            assert call[0][1]["tenant_id"] == "persistent-tenant"

    @pytest.mark.asyncio
    async def test_different_wrappers_have_different_tenant_ids(self):
        """Verify multiple wrapper instances maintain separate tenant_id values."""
        # Arrange
        mock_mcp = AsyncMock()
        mock_mcp.return_value = {}
        wrapper1 = TenantInjectorMCPClient("tenant-1", mock_mcp)
        wrapper2 = TenantInjectorMCPClient("tenant-2", mock_mcp)

        # Act
        await wrapper1("search-documents", {"query": "test"})
        await wrapper2("search-documents", {"query": "test"})

        # Assert
        calls = mock_mcp.call_args_list
        assert calls[0][0][1]["tenant_id"] == "tenant-1"
        assert calls[1][0][1]["tenant_id"] == "tenant-2"


class TestSystemPrompt:
    """Test suite for system prompt correctness."""

    def test_system_prompt_includes_compare_documents_workflow(self):
        """Verify prompt explains compare-documents requires document IDs from metadata."""
        from agent import SYSTEM_PROMPT

        # Check that the prompt mentions the workflow
        assert "compare-documents" in SYSTEM_PROMPT
        assert "document_id_a" in SYSTEM_PROMPT
        assert "document_id_b" in SYSTEM_PROMPT
        assert "get-document-metadata FIRST" in SYSTEM_PROMPT or "call get-document-metadata first" in SYSTEM_PROMPT.lower()

    def test_system_prompt_explains_document_ids_are_uuids(self):
        """Verify prompt explains document IDs are UUIDs, not filenames."""
        from agent import SYSTEM_PROMPT

        assert "UUID" in SYSTEM_PROMPT
        assert "NOT filename" in SYSTEM_PROMPT or "not filename" in SYSTEM_PROMPT.lower()

    def test_system_prompt_lists_all_tool_parameters(self):
        """Verify prompt documents all required tool parameters."""
        from agent import SYSTEM_PROMPT

        # search-documents parameters
        assert "search-documents" in SYSTEM_PROMPT
        assert "query" in SYSTEM_PROMPT

        # get-document-metadata parameters
        assert "get-document-metadata" in SYSTEM_PROMPT
        assert "document_id" in SYSTEM_PROMPT

        # compare-documents parameters
        assert "compare-documents" in SYSTEM_PROMPT
        assert "document_id_a" in SYSTEM_PROMPT
        assert "document_id_b" in SYSTEM_PROMPT
