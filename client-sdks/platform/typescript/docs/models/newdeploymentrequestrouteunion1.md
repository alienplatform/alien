# NewDeploymentRequestRouteUnion1

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.NewDeploymentRequestRouteIngress1`

```typescript
const value: models.NewDeploymentRequestRouteIngress1 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.NewDeploymentRequestRouteGateway1`

```typescript
const value: models.NewDeploymentRequestRouteGateway1 = {
  gatewayClassName: "<value>",
  listenerPort: 953962,
  routeApi: "gateway",
};
```

