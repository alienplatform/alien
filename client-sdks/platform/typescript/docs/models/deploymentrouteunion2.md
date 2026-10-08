# DeploymentRouteUnion2

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.DeploymentRouteIngress2`

```typescript
const value: models.DeploymentRouteIngress2 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.DeploymentRouteGateway2`

```typescript
const value: models.DeploymentRouteGateway2 = {
  gatewayClassName: "<value>",
  listenerPort: 894718,
  routeApi: "gateway",
};
```

