# RouteUnion2

Kubernetes route API selected for public endpoints.


## Supported Types

### `operations.RouteIngress2`

```typescript
const value: operations.RouteIngress2 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `operations.RouteGateway2`

```typescript
const value: operations.RouteGateway2 = {
  gatewayClassName: "<value>",
  listenerPort: 271202,
  routeApi: "gateway",
};
```

