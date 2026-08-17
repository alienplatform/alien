# TargetDeploymentRouteUnion2

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.TargetDeploymentRouteIngress2`

```typescript
const value: models.TargetDeploymentRouteIngress2 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.TargetDeploymentRouteGateway2`

```typescript
const value: models.TargetDeploymentRouteGateway2 = {
  gatewayClassName: "<value>",
  listenerPort: 68786,
  routeApi: "gateway",
};
```

