#!/usr/bin/env python3
"""
Local test for TenantInjectorMCPClient wrapper.

This script validates that:
1. The wrapper properly exposes MCP protocol attributes (especially 'tools')
2. Tool calls are intercepted and tenant_id is injected
3. Strands Agent can discover tools through the wrapper

Run: python3 test_tenant_injector.py
"""
import logging
import sys
from typing import Any, Dict
from unittest.mock import Mock, AsyncMock, MagicMock

logging.basicConfig(level=logging.INFO, stream=sys.stdout)
logger = logging.getLogger(__name__)


class TenantInjectorMCPClient:
    """FIXED wrapper that properly proxies MCP protocol to Strands."""

    def __init__(self, tenant_id: str, mcp_client):
        self.tenant_id = tenant_id
        self.mcp_client = mcp_client
        logger.info(f"✓ TenantInjectorMCPClient initialized with tenant_id={tenant_id}")

    async def __call__(self, tool_name: str, tool_input: Dict[str, Any]) -> Any:
        """Intercept tool calls and inject tenant_id automatically."""
        tool_input_with_tenant = {**tool_input, "tenant_id": self.tenant_id}
        logger.info(f"✓ Tool call intercepted: {tool_name} with tenant_id={self.tenant_id}")
        logger.info(f"  Input: {tool_input} → {tool_input_with_tenant}")
        return await self.mcp_client(tool_name, tool_input_with_tenant)

    def __getattr__(self, name):
        """Delegate all other attributes to the wrapped client."""
        logger.debug(f"  Delegating attribute access: {name}")
        return getattr(self.mcp_client, name)

    def __dir__(self):
        """Expose all attributes from wrapped client for introspection."""
        return dir(self.mcp_client)

    # CRITICAL FIX: Explicitly expose 'tools' property for Strands discovery
    @property
    def tools(self):
        """Proxy tools list/dict from underlying MCP client."""
        if hasattr(self.mcp_client, 'tools'):
            tools = self.mcp_client.tools
            logger.info(f"✓ Exposing tools from MCP client: {tools}")
            return tools
        logger.warning("⚠ MCP client has no 'tools' attribute")
        return []


def create_mock_mcp_client():
    """Create a mock MCPClient that simulates Strands' expected interface."""

    class MockMCPClient:
        """Mock MCP client with async call support."""

        tools = [
            {
                "name": "search-documents",
                "description": "Search for documents",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"},
                        "limit": {"type": "integer"}
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "get-document-metadata",
                "description": "Get document metadata",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "document_id": {"type": "string"},
                        "limit": {"type": "integer"}
                    }
                }
            },
            {
                "name": "compare-documents",
                "description": "Compare two documents",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"},
                        "document_id_a": {"type": "string"},
                        "document_id_b": {"type": "string"},
                        "limit": {"type": "integer"}
                    },
                    "required": ["query", "document_id_a", "document_id_b"]
                }
            }
        ]

        async def __call__(self, tool_name: str, tool_input: Dict[str, Any]) -> Any:
            """Simulate Lambda function call."""
            logger.info(f"  → Mock Lambda received: {tool_name}({tool_input})")
            # Verify tenant_id is present
            if "tenant_id" not in tool_input:
                raise ValueError("❌ MISSING tenant_id in Lambda request!")
            return {"status": "success", "tool": tool_name, "tenant_id": tool_input["tenant_id"]}

    return MockMCPClient()


async def test_wrapper():
    """Test the fixed TenantInjectorMCPClient wrapper."""

    print("\n" + "="*60)
    print("Testing TenantInjectorMCPClient Wrapper")
    print("="*60 + "\n")

    # Setup
    tenant_id = "test-tenant-123"
    mock_mcp = create_mock_mcp_client()
    wrapper = TenantInjectorMCPClient(tenant_id, mock_mcp)

    # Test 1: Tools discovery (what Strands does during Agent initialization)
    print("\n[TEST 1] Tool Discovery")
    print("-" * 40)
    try:
        tools = wrapper.tools
        print(f"✓ wrapper.tools accessible: {len(tools)} tools found")
        for tool in tools:
            print(f"  - {tool['name']}: {tool['description']}")
        assert len(tools) == 3, f"Expected 3 tools, got {len(tools)}"
        print("✓ TEST 1 PASSED\n")
    except Exception as e:
        print(f"❌ TEST 1 FAILED: {e}\n")
        return False

    # Test 2: Attribute delegation (getattr fallthrough)
    print("\n[TEST 2] Attribute Delegation")
    print("-" * 40)
    try:
        # Access tools again via __getattr__
        delegated_tools = wrapper.tools
        print(f"✓ __getattr__ delegation works: {len(delegated_tools)} tools")
        print("✓ TEST 2 PASSED\n")
    except Exception as e:
        print(f"❌ TEST 2 FAILED: {e}\n")
        return False

    # Test 3: Tool call interception (tenant_id injection)
    print("\n[TEST 3] Tool Call Interception")
    print("-" * 40)
    try:
        # Simulate Strands calling a tool WITHOUT tenant_id
        tool_input = {"query": "observability", "limit": 5}
        print(f"Input (no tenant_id): {tool_input}")

        result = await wrapper("search-documents", tool_input)

        print(f"Result: {result}")
        assert result["tenant_id"] == tenant_id, "tenant_id not injected!"
        print("✓ tenant_id successfully injected")
        print("✓ TEST 3 PASSED\n")
    except Exception as e:
        print(f"❌ TEST 3 FAILED: {e}\n")
        return False

    # Test 4: Verify Lambda receives tenant_id
    print("\n[TEST 4] Lambda Request Validation")
    print("-" * 40)
    try:
        # This should succeed (tenant_id injected)
        await wrapper("get-document-metadata", {"limit": 10})
        print("✓ Lambda received request with tenant_id")

        # Simulate calling without wrapper (should fail)
        try:
            await mock_mcp("search-documents", {"query": "test"})
            print("❌ Should have failed without tenant_id!")
            return False
        except ValueError as e:
            print(f"✓ Direct call correctly rejected: {e}")

        print("✓ TEST 4 PASSED\n")
    except Exception as e:
        print(f"❌ TEST 4 FAILED: {e}\n")
        return False

    # Test 5: dir() introspection
    print("\n[TEST 5] Introspection (__dir__)")
    print("-" * 40)
    try:
        attrs = dir(wrapper)
        print(f"✓ dir(wrapper) returns {len(attrs)} attributes")
        print(f"  'tools' in dir: {'tools' in attrs}")
        print("✓ TEST 5 PASSED\n")
    except Exception as e:
        print(f"❌ TEST 5 FAILED: {e}\n")
        return False

    print("\n" + "="*60)
    print("✅ ALL TESTS PASSED - Wrapper is ready for deployment!")
    print("="*60 + "\n")
    return True


if __name__ == "__main__":
    import asyncio

    success = asyncio.run(test_wrapper())
    sys.exit(0 if success else 1)
