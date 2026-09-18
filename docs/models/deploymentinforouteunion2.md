# DeploymentInfoRouteUnion2

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.DeploymentInfoRouteIngress2`

```typescript
const value: models.DeploymentInfoRouteIngress2 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.DeploymentInfoRouteGateway2`

```typescript
const value: models.DeploymentInfoRouteGateway2 = {
  gatewayClassName: "<value>",
  listenerPort: 907985,
  routeApi: "gateway",
};
```

