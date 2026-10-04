# ExternalBindingUnion4

Service-type based artifact registry binding that supports multiple registry providers


## Supported Types

### `models.ExternalBindingEcr`

```typescript
const value: models.ExternalBindingEcr = {
  repositoryPrefix: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  service: "ecr",
  type: "artifact_registry",
};
```

### `models.ExternalBindingAcr`

```typescript
const value: models.ExternalBindingAcr = {
  registryName: "<value>",
  resourceGroupName: "<value>",
  service: "acr",
  type: "artifact_registry",
};
```

### `models.ExternalBindingGar`

```typescript
const value: models.ExternalBindingGar = {
  repositoryName: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  service: "gar",
  type: "artifact_registry",
};
```

### `models.ExternalBindingLocal`

```typescript
const value: models.ExternalBindingLocal = {
  dataDir: "<value>",
  registryUrl: "https://fixed-alligator.org",
  service: "local",
  type: "artifact_registry",
};
```

