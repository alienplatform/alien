# CreateManagerResponseRouteUnion4

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.CreateManagerResponseRouteIngress4`

```typescript
const value: models.CreateManagerResponseRouteIngress4 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.CreateManagerResponseRouteGateway4`

```typescript
const value: models.CreateManagerResponseRouteGateway4 = {
  gatewayClassName: "<value>",
  listenerPort: 20415,
  routeApi: "gateway",
};
```

