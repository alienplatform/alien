# CloudFormationCallbackRequestRouteUnion1

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.CloudFormationCallbackRequestRouteIngress1`

```typescript
const value: models.CloudFormationCallbackRequestRouteIngress1 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.CloudFormationCallbackRequestRouteGateway1`

```typescript
const value: models.CloudFormationCallbackRequestRouteGateway1 = {
  gatewayClassName: "<value>",
  listenerPort: 534202,
  routeApi: "gateway",
};
```

