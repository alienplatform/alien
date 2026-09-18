# TargetDeploymentPoolsUnion

User-selected deployment settings for one compute pool.


## Supported Types

### `models.TargetDeploymentPoolsFixed`

```typescript
const value: models.TargetDeploymentPoolsFixed = {
  machines: 184184,
  mode: "fixed",
};
```

### `models.TargetDeploymentPoolsAutoscale`

```typescript
const value: models.TargetDeploymentPoolsAutoscale = {
  max: 645486,
  min: 887609,
  mode: "autoscale",
};
```

