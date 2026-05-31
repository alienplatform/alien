# KubernetesRouteProfile

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.KubernetesRouteProfileIngress`

```typescript
const value: models.KubernetesRouteProfileIngress = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.KubernetesRouteProfileGateway`

```typescript
const value: models.KubernetesRouteProfileGateway = {
  gatewayClassName: "<value>",
  listenerPort: 701479,
  routeApi: "gateway",
};
```

