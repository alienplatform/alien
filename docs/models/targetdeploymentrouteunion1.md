# TargetDeploymentRouteUnion1

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.TargetDeploymentRouteIngress1`

```typescript
const value: models.TargetDeploymentRouteIngress1 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.TargetDeploymentRouteGateway1`

```typescript
const value: models.TargetDeploymentRouteGateway1 = {
  gatewayClassName: "<value>",
  listenerPort: 672062,
  routeApi: "gateway",
};
```

