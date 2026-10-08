# CreateSetupRegistrationOperationRequestRouteUnion2

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.CreateSetupRegistrationOperationRequestRouteIngress2`

```typescript
const value: models.CreateSetupRegistrationOperationRequestRouteIngress2 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.CreateSetupRegistrationOperationRequestRouteGateway2`

```typescript
const value: models.CreateSetupRegistrationOperationRequestRouteGateway2 = {
  gatewayClassName: "<value>",
  listenerPort: 265048,
  routeApi: "gateway",
};
```

