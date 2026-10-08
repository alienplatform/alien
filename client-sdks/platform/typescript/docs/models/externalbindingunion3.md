# ExternalBindingUnion3

Represents a KV binding for key-value storage across platforms


## Supported Types

### `models.ExternalBindingDynamodb`

```typescript
const value: models.ExternalBindingDynamodb = {
  region: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  tableName: "<value>",
  service: "dynamodb",
  type: "kv",
};
```

### `models.ExternalBindingFirestore`

```typescript
const value: models.ExternalBindingFirestore = {
  collectionName: "<value>",
  databaseId: "<id>",
  projectId: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  service: "firestore",
  type: "kv",
};
```

### `models.ExternalBindingTablestorage`

```typescript
const value: models.ExternalBindingTablestorage = {
  accountName: "<value>",
  resourceGroupName: "<value>",
  tableName: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  service: "tablestorage",
  type: "kv",
};
```

### `models.ExternalBindingRedis`

```typescript
const value: models.ExternalBindingRedis = {
  connectionUrl: "https://smoggy-consistency.info",
  service: "redis",
  type: "kv",
};
```

### `models.ExternalBindingLocalKv`

```typescript
const value: models.ExternalBindingLocalKv = {
  dataDir: "<value>",
  service: "local-kv",
  type: "kv",
};
```
