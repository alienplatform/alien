# SyncListResponseRouteUnion2

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.SyncListResponseRouteIngress2`

```typescript
const value: models.SyncListResponseRouteIngress2 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.SyncListResponseRouteGateway2`

```typescript
const value: models.SyncListResponseRouteGateway2 = {
  gatewayClassName: "<value>",
  listenerPort: 456554,
  routeApi: "gateway",
};
```

