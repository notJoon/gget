# Package Manager Design Document

## 1. Overview

The Gno Package Manager is a tool for downloading and managing packages through Gno.land RPC endpoints.

## 2. Core Features

1. Package downloading
2. Package dependency management
3. Package version management
4. Local cache management

## 3. API Design

### 3.1 RPC Endpoint

```plain
Base URL: https://rpc.gno.land:443
Method: POST
Content-Type: application/json
```

### 3.2 Request Format

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "abci_query",
  "params": {
    "path": "vm/qfile",
    "data": "<base64_encoded_package_path>"
  }
}
```

### Request Examples

1. Package List Query:

```bash
# Original package path: gno.land/p/demo/avl
# base64 encoded: Z25vLmxhbmQvcC9kZW1vL2F2bA==

curl -X POST "https://rpc.gno.land:443" \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "abci_query",
    "params": {
      "path": "vm/qfile",
      "data": "Z25vLmxhbmQvcC9kZW1vL2F2bA=="
    }
  }'
```

2. Specific File Query:

```bash
# Original file path: gno.land/p/demo/avl/node.gno
# base64 encoded: Z25vLmxhbmQvcC9kZW1vL2F2bC9ub2RlLmdubw==

curl -X POST "https://rpc.gno.land:443" \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "abci_query",
    "params": {
      "path": "vm/qfile",
      "data": "Z25vLmxhbmQvcC9kZW1vL2F2bC9ub2RlLmdubw=="
    }
  }'
```

### 3.3 Response Format

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "response": {
      "ResponseBase": {
        "Error": null,
        "Data": "<base64_encoded_file_content>",
        "Events": null,
        "Log": "",
        "Info": ""
      }
    }
  }
}
```

### Response Examples

1. Package List Query Response:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "response": {
      "ResponseBase": {
        "Error": null,
        "Data": "bm9kZS5nbm8Kbm9kZV90ZXN0Lmdubwp0cmVlLmdubwp0cmVlX3Rlc3QuZ25vCnpfMF9maWxldGVzdC5nbm8Kel8xX2ZpbGV0ZXN0Lmdubwp6XzJfZmlsZXRlc3QuZ25v",
        "Events": null,
        "Log": "",
        "Info": ""
      }
    }
  }
}
```

```plain
# base64 decoded Data field:
node.gno
node_test.gno
tree.gno
tree_test.gno
z_0_filetest.gno
z_1_filetest.gno
z_2_filetest.gno
```

2. Specific File Query Response:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "response": {
      "ResponseBase": {
        "Error": null,
        "Data": "cGFja2FnZSBhdmwKCi8vLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLS0tLQovLyBOb2RlCgovLyBOb2RlIHJlcHJlc2VudHMgYSBub2RlIGluIGFuIEFWTCB0cmVlLgp0eXBlIE5vZGUgc3RydWN0IHsKCWtleSAgICAgICBzdHJpbmcgLy8ga2V5IGlzIHRoZSB1bmlxdWUgaWRlbnRpZmllciBmb3IgdGhlIG5vZGUuCgl2YWx1ZSAgICAgYW55ICAgIC8vIHZhbHVlIGlzIHRoZSBkYXRhIHN0b3JlZCBpbiB0aGUgbm9kZS4KCWhlaWdodCAgICBpbnQ4ICAgLy8gaGVpZ2h0IGlzIHRoZSBoZWlnaHQgb2YgdGhlIG5vZGUgaW4gdGhlIHRyZWUuCglzaXplICAgICAgaW50ICAgIC8vIHNpemUgaXMgdGhlIG51bWJlciBvZiBub2RlcyBpbiB0aGUgc3VidHJlZSByb290ZWQgYXQgdGhpcyBub2RlLgoJbGVmdE5vZGUgICpOb2RlICAvLyBsZWZ0Tm9kZSBpcyB0aGUgbGVmdCBjaGlsZCBvZiB0aGUgbm9kZS4KCXJpZ2h0Tm9kZSAqTm9kZSAgLy8gcmlnaHROb2RlIGlzIHRoZSByaWdodCBjaGlsZCBvZiB0aGUgbm9kZS4KfQ==",
        "Events": null,
        "Log": "",
        "Info": ""
      }
    }
  }
}
```

# base64 decoded Data field:

```go
package avl

//-------------------
// Node

// Node represents a node in an AVL tree.
type Node struct {
	key       string // key is the unique identifier for the node.
	value     any    // value is the data stored in the node.
	height    int8   // height is the height of the node in the tree.
	size      int    // size is the number of nodes in the subtree rooted at this node.
	leftNode  *Node  // leftNode is the left child of the node.
	rightNode *Node  // rightNode is the right child of the node.
}

...
```

## 4. Package Download Process

### 4.1 Package List Retrieval

1. Encode package path in base64
2. Send query to RPC endpoint
3. Decode response data from base64 to obtain file list

### 4.2 File Download

1. For each file:
   - Encode file path in base64
   - Send query to RPC endpoint
   - Decode response data from base64 to obtain file content
   - Save to local file system

## 5. Implementation Considerations

### 5.1 Error Handling

- RPC connection failure
- Invalid package path
- File download failure
- Base64 encoding/decoding errors

### 5.2 Performance Optimization

- Implement parallel downloads
- Utilize local cache
- Retry mechanisms

### 5.3 Security

- RPC endpoint validation
- Downloaded file integrity verification
- Secure file system access

## 6. Future Improvements

1. Add package version management functionality
2. Implement dependency resolution mechanism
3. Package signing and verification features
4. Offline mode support
5. Package update mechanism

This design document describes the basic structure and operation of a package manager. Actual implementation may require more detailed specifications and exception handling.
