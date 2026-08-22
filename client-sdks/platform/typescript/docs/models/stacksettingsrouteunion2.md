# StackSettingsRouteUnion2

Kubernetes route API selected for public endpoints.

## Supported Types

### `models.StackSettingsRouteIngress2`

```typescript
const value: models.StackSettingsRouteIngress2 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.StackSettingsRouteGateway2`

```typescript
const value: models.StackSettingsRouteGateway2 = {
  gatewayClassName: "<value>",
  listenerPort: 121536,
  routeApi: "gateway",
};
```
