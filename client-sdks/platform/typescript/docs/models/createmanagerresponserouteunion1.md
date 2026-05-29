# CreateManagerResponseRouteUnion1

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.CreateManagerResponseRouteIngress1`

```typescript
const value: models.CreateManagerResponseRouteIngress1 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.CreateManagerResponseRouteGateway1`

```typescript
const value: models.CreateManagerResponseRouteGateway1 = {
  gatewayClassName: "<value>",
  listenerPort: 532940,
  routeApi: "gateway",
};
```

