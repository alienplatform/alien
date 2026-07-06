# BYOC Database

A zero-disk vector database that runs in the customer's cloud. This is the BYOC (Bring Your Own Cloud) pattern for data infrastructure: you control the releases, the customer owns the data. All containers are stateless -- object storage is the only source of truth, so the same code deploys unchanged to AWS S3, GCP Cloud Storage, Azure Blob Storage, or the local filesystem.

## Architecture

- **Writer**: handles vector upserts, writes segments to object storage
- **Reader**: handles similarity queries, reads from object storage
- **Router**: nginx, routes requests to writer or reader by endpoint
- **Storage**: durable object storage for vectors and metadata

Coordination happens through ETags on object storage. There are no distributed locks and no consensus protocols: when multiple writers compete to update metadata, ETag-based optimistic locking ensures consistency. Kill any container and restart it -- the data is still there, because no container ever held state.

## Local development

```bash
git clone https://github.com/alienplatform/alien
cd alien/examples/byoc-database

npm install
alien dev
```

Everything runs locally -- object storage on the filesystem, no cloud credentials needed. The database listens on `http://localhost:8080`.

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

## Running tests

```bash
npm test
```

Tests cover upsert and query operations, data persistence across container restarts, namespace isolation, dimension validation, and error handling.

## Production considerations

This example simplifies several things for clarity:

1. **Index caching**: production would cache vector indexes in memory and invalidate on segment changes
2. **Buffered writes**: production would buffer vectors and flush in larger batches
3. **Segment compaction**: production would merge small segments to optimize query performance
4. **Distributed queries**: production would shard namespaces and query in parallel
5. **Authentication**: production would add API authentication and authorization

The fundamentals remain: stateless containers plus ETag-based coordination on object storage.

## Learn more

- [How Alien Works](https://alien.dev/docs/how-alien-works)
- [Storage reference](https://alien.dev/docs/infrastructure/storage)
- [alien.dev](https://alien.dev) -- ship to your customer's cloud, keep it fully managed
