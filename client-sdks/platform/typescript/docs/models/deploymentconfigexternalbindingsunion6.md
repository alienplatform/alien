# DeploymentConfigExternalBindingsUnion6

Connection details for a Postgres database, one variant per backend.


## Supported Types

### `models.DeploymentConfigExternalBindingsAurora`

```typescript
const value: models.DeploymentConfigExternalBindingsAurora = {
  service: "aurora",
  type: "postgres",
};
```

### `models.DeploymentConfigExternalBindingsCloudSQL`

```typescript
const value: models.DeploymentConfigExternalBindingsCloudSQL = {
  service: "cloud-sql",
  type: "postgres",
};
```

### `models.DeploymentConfigExternalBindingsFlexibleServer`

```typescript
const value: models.DeploymentConfigExternalBindingsFlexibleServer = {
  service: "flexible-server",
  type: "postgres",
};
```

### `models.DeploymentConfigExternalBindingsExternal`

```typescript
const value: models.DeploymentConfigExternalBindingsExternal = {
  password: "t7NGcII7QRzEiYJ",
  service: "external",
  type: "postgres",
};
```

### `models.DeploymentConfigExternalBindingsLocalPostgres`

```typescript
const value: models.DeploymentConfigExternalBindingsLocalPostgres = {
  password: "VvdP7qKtXHlFBzh",
  service: "local-postgres",
  type: "postgres",
};
```

