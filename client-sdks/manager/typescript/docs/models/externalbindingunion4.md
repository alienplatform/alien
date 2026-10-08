# ExternalBindingUnion4

External artifact registry binding (OCI registry)


## Supported Types

### `models.ExternalBindingEcr`

```typescript
const value: models.ExternalBindingEcr = {
  repositoryPrefix: {
    secretRef: {
      key: "<key>",
      name: "<value>",
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
  repositoryName: "<value>",
  service: "gar",
  type: "artifact_registry",
};
```

### `models.ExternalBindingLocal`

```typescript
const value: models.ExternalBindingLocal = {
  dataDir: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  registryUrl: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  service: "local",
  type: "artifact_registry",
};
```
