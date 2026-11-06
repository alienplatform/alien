# KubernetesExposureCustomBaseDomainRouteUnion

Kubernetes route API selected for public endpoints.


## Supported Types

### `models.KubernetesExposureCustomBaseDomainRouteIngress`

```typescript
const value: models.KubernetesExposureCustomBaseDomainRouteIngress = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

### `models.KubernetesExposureCustomBaseDomainRouteGateway`

```typescript
const value: models.KubernetesExposureCustomBaseDomainRouteGateway = {
  gatewayClassName: "<value>",
  listenerPort: 908997,
  routeApi: "gateway",
};
```
