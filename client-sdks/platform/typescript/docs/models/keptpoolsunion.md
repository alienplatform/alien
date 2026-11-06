# KeptPoolsUnion

User-selected deployment settings for one compute pool.


## Supported Types

### `models.PoolsKeptFixed`

```typescript
const value: models.PoolsKeptFixed = {
  machines: 577271,
  mode: "fixed",
};
```

### `models.PoolsKeptAutoscale`

```typescript
const value: models.PoolsKeptAutoscale = {
  max: 146340,
  min: 30172,
  mode: "autoscale",
};
```
