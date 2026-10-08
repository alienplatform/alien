# ExternalBindingUnion3

External KV binding (Redis, etc.)


## Supported Types

### `models.ExternalBindingDynamodb`

```typescript
const value: models.ExternalBindingDynamodb = {
  region: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  tableName: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  service: "dynamodb",
  type: "kv",
};
```

### `models.ExternalBindingFirestore`

```typescript
const value: models.ExternalBindingFirestore = {
  collectionName: "<value>",
  databaseId: "<id>",
  projectId: "<id>",
  service: "firestore",
  type: "kv",
};
```

### `models.ExternalBindingTablestorage`

```typescript
const value: models.ExternalBindingTablestorage = {
  accountName: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  resourceGroupName: "<value>",
  tableName: "<value>",
  service: "tablestorage",
  type: "kv",
};
```

### `models.ExternalBindingRedis`

```typescript
const value: models.ExternalBindingRedis = {
  connectionUrl: "https://warm-ribbon.biz",
  service: "redis",
  type: "kv",
};
```

### `models.ExternalBindingLocalKv`

```typescript
const value: models.ExternalBindingLocalKv = {
  dataDir: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  service: "local-kv",
  type: "kv",
};
```
