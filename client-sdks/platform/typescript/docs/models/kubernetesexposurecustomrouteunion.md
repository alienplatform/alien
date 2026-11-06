# KubernetesExposureCustomRouteUnion

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.KubernetesExposureCustomRouteIngress`

```typescript
const value: models.KubernetesExposureCustomRouteIngress = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.KubernetesExposureCustomRouteGateway`

```typescript
const value: models.KubernetesExposureCustomRouteGateway = {
  gatewayClassName: "<value>",
  listenerPort: 901379,
  routeApi: "gateway",
};
```
