# DeploymentConfigExternalBindingsUnion2

Binding parameters for Queue at runtime or in templates.


## Supported Types

### `models.DeploymentConfigExternalBindingsSqs`

```typescript
const value: models.DeploymentConfigExternalBindingsSqs = {
  service: "sqs",
  type: "queue",
};
```

### `models.DeploymentConfigExternalBindingsPubsub`

```typescript
const value: models.DeploymentConfigExternalBindingsPubsub = {
  service: "pubsub",
  type: "queue",
};
```

### `models.DeploymentConfigExternalBindingsServicebus`

```typescript
const value: models.DeploymentConfigExternalBindingsServicebus = {
  service: "servicebus",
  type: "queue",
};
```

### `models.DeploymentConfigExternalBindingsLocalQueue`

```typescript
const value: models.DeploymentConfigExternalBindingsLocalQueue = {
  service: "local-queue",
  type: "queue",
};
```

