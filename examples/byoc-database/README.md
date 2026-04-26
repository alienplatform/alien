# BYOC Database

A multi-container vector database example that runs in the customer's cloud. Demonstrates the BYOC (Bring Your Own Cloud) pattern with stateless containers and durable object storage.

## Architecture

- **Writer**: Handles vector upsert operations, writes segments to object storage
- **Reader**: Handles vector similarity queries, reads from object storage
- **Router**: nginx-based routing to writer/reader based on endpoint
- **Storage**: Durable object storage for vectors and metadata

The key insight: **coordination via ETags on object storage**. No distributed locks, no consensus protocols. When multiple writers compete to update metadata, ETag-based optimistic locking ensures consistency.

## Local Development

```bash
# Install dependencies
npm install

# Run in dev mode
alien dev
```

The stack starts on `http://localhost:8080`.

## API Usage

### Upsert vectors

```bash
curl -X POST http://localhost:8080/api/v1/namespaces/demo/upsert \
  -H "Content-Type: application/json" \
  -d '{
    "vectors": [
      {"id": "doc1", "values": [0.1, 0.2, 0.3, 0.4], "metadata": {"title": "Hello"}},
      {"id": "doc2", "values": [0.2, 0.3, 0.4, 0.5], "metadata": {"title": "World"}},
      {"id": "doc3", "values": [0.9, 0.8, 0.7, 0.6], "metadata": {"title": "Other"}}
    ]
  }'
```

### Query by similarity

```bash
curl -X POST http://localhost:8080/api/v1/namespaces/demo/query \
  -H "Content-Type: application/json" \
  -d '{"vector": [0.1, 0.2, 0.3, 0.4], "topK": 2}'
```

Response:
```json
{
  "results": [
    {"id": "doc1", "score": 1.0, "metadata": {"title": "Hello"}},
    {"id": "doc2", "score": 0.98, "metadata": {"title": "World"}}
  ]
}
```

## Testing

```bash
npm test
```

Tests verify:
- Vector upsert and query operations
- Data persistence across container restarts
- Namespace isolation
- Dimension validation
- Error handling

## Key Concepts for Presenters

**This is BYOC**: We control the releases, the customer owns the data.

**Three containers**: Writer, reader, router. All stateless. Object storage is the source of truth.

**Restart resilience**: Kill the reader, query again, same data. No state migration needed.

**ETag-based coordination**: No Zookeeper, no etcd, no DynamoDB locks. Just object storage primitives.

**Platform agnostic**: Same code deploys to AWS S3, GCP Cloud Storage, Azure Blob, or local filesystem.

## Production Considerations

This example simplifies several things for clarity:

1. **Index caching**: Production would cache vector indexes in memory and invalidate on segment changes
2. **Buffered writes**: Production would buffer vectors and flush in larger batches
3. **Segment compaction**: Production would merge small segments to optimize query performance
4. **Distributed queries**: Production would shard namespaces and query in parallel
5. **Authentication**: Production would add API authentication and authorization

The fundamentals remain: stateless containers + ETag-based coordination on object storage.


