# DeploymentPoolsUnion

User-selected deployment settings for one compute pool.


## Supported Types

### `models.DeploymentPoolsFixed`

```typescript
const value: models.DeploymentPoolsFixed = {
  machines: 563700,
  mode: "fixed",
};
```

### `models.DeploymentPoolsAutoscale`

```typescript
const value: models.DeploymentPoolsAutoscale = {
  max: 164752,
  min: 220429,
  mode: "autoscale",
};
```

