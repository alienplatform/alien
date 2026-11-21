# DeploymentRouteUnion1

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.DeploymentRouteIngress1`

```typescript
const value: models.DeploymentRouteIngress1 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.DeploymentRouteGateway1`

```typescript
const value: models.DeploymentRouteGateway1 = {
  gatewayClassName: "<value>",
  listenerPort: 966900,
  routeApi: "gateway",
};
```

