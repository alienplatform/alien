# AzureCredentials

Represents Azure authentication credentials


## Supported Types

### `models.AzureCredentialsServicePrincipal`

```typescript
const value: models.AzureCredentialsServicePrincipal = {
  clientId: "<id>",
  clientSecret: "<value>",
  type: "servicePrincipal",
};
```

### `models.AzureCredentialsAccessToken`

```typescript
const value: models.AzureCredentialsAccessToken = {
  token: "<value>",
  type: "accessToken",
};
```

### `models.AzureCredentialsScopedAccessTokens`

```typescript
const value: models.AzureCredentialsScopedAccessTokens = {
  tokens: {
    "key": "<value>",
    "key1": "<value>",
    "key2": "<value>",
  },
  type: "scopedAccessTokens",
};
```

### `models.AzureCredentialsSasToken`

```typescript
const value: models.AzureCredentialsSasToken = {
  queryParameters: {},
  type: "sasToken",
};
```

### `models.AzureCredentialsVMManagedIdentity`

```typescript
const value: models.AzureCredentialsVMManagedIdentity = {
  clientId: "<id>",
  type: "vmManagedIdentity",
};
```

### `models.AzureCredentialsWorkloadIdentity`

```typescript
const value: models.AzureCredentialsWorkloadIdentity = {
  authorityHost: "<value>",
  clientId: "<id>",
  federatedTokenFile: "<value>",
  tenantId: "<id>",
  type: "workloadIdentity",
};
```

### `models.AzureCredentialsManagedIdentity`

```typescript
const value: models.AzureCredentialsManagedIdentity = {
  clientId: "<id>",
  identityEndpoint: "<value>",
  identityHeader: "<value>",
  type: "managedIdentity",
};
```

