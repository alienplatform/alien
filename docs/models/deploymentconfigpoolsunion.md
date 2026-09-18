# DeploymentConfigPoolsUnion

User-selected deployment settings for one compute pool.


## Supported Types

### `models.DeploymentConfigPoolsFixed`

```typescript
const value: models.DeploymentConfigPoolsFixed = {
  machines: 352245,
  mode: "fixed",
};
```

### `models.DeploymentConfigPoolsAutoscale`

```typescript
const value: models.DeploymentConfigPoolsAutoscale = {
  max: 706380,
  min: 333321,
  mode: "autoscale",
};
```

