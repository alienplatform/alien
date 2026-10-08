# CreateSetupRegistrationOperationRequestRouteUnion1

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.CreateSetupRegistrationOperationRequestRouteIngress1`

```typescript
const value: models.CreateSetupRegistrationOperationRequestRouteIngress1 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.CreateSetupRegistrationOperationRequestRouteGateway1`

```typescript
const value: models.CreateSetupRegistrationOperationRequestRouteGateway1 = {
  gatewayClassName: "<value>",
  listenerPort: 840342,
  routeApi: "gateway",
};
```

