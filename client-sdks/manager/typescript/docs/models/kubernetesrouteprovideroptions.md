# KubernetesRouteProviderOptions

Provider-specific route options required by supported managed profiles.


## Supported Types

### `models.KubernetesRouteProviderOptionsAwsAlb`

```typescript
const value: models.KubernetesRouteProviderOptionsAwsAlb = {
  provider: "awsAlb",
  scheme: "<value>",
  targetType: "<value>",
};
```

### `models.KubernetesRouteProviderOptionsGkeGateway`

```typescript
const value: models.KubernetesRouteProviderOptionsGkeGateway = {
  provider: "gkeGateway",
};
```

### `models.KubernetesRouteProviderOptionsAzureApplicationGatewayForContainers`

```typescript
const value:
  models.KubernetesRouteProviderOptionsAzureApplicationGatewayForContainers = {
    frontend: "<value>",
    provider: "azureApplicationGatewayForContainers",
  };
```

