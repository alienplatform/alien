# DeploymentConfigRouteUnion1

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.DeploymentConfigRouteIngress1`

```typescript
const value: models.DeploymentConfigRouteIngress1 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.DeploymentConfigRouteGateway1`

```typescript
const value: models.DeploymentConfigRouteGateway1 = {
  gatewayClassName: "<value>",
  listenerPort: 246261,
  routeApi: "gateway",
};
```

