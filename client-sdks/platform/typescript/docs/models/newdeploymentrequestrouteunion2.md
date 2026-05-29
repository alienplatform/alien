# NewDeploymentRequestRouteUnion2

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.NewDeploymentRequestRouteIngress2`

```typescript
const value: models.NewDeploymentRequestRouteIngress2 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.NewDeploymentRequestRouteGateway2`

```typescript
const value: models.NewDeploymentRequestRouteGateway2 = {
  gatewayClassName: "<value>",
  listenerPort: 25193,
  routeApi: "gateway",
};
```

