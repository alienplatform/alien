# DeploymentConfigRouteUnion2

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.DeploymentConfigRouteIngress2`

```typescript
const value: models.DeploymentConfigRouteIngress2 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.DeploymentConfigRouteGateway2`

```typescript
const value: models.DeploymentConfigRouteGateway2 = {
  gatewayClassName: "<value>",
  listenerPort: 147109,
  routeApi: "gateway",
};
```

