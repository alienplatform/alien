# CreateManagerResponseRouteUnion2

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.CreateManagerResponseRouteIngress2`

```typescript
const value: models.CreateManagerResponseRouteIngress2 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.CreateManagerResponseRouteGateway2`

```typescript
const value: models.CreateManagerResponseRouteGateway2 = {
  gatewayClassName: "<value>",
  listenerPort: 284306,
  routeApi: "gateway",
};
```

