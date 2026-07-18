# GcpCredentials

Authentication options for talking to GCP APIs.


## Supported Types

### `models.GcpCredentialsAccessToken`

```typescript
const value: models.GcpCredentialsAccessToken = {
  token: "<value>",
  type: "accessToken",
};
```

### `models.GcpCredentialsImpersonatedServiceAccount`

```typescript
const value: models.GcpCredentialsImpersonatedServiceAccount = {
  config: {
    scopes: [
      "<value 1>",
    ],
    serviceAccountEmail: "<value>",
  },
  source: {},
  type: "impersonatedServiceAccount",
};
```

### `models.GcpCredentialsServiceAccountKey`

```typescript
const value: models.GcpCredentialsServiceAccountKey = {
  json: "{key: 7023388629692829, key1: null, key2: \"<value>\"}",
  type: "serviceAccountKey",
};
```

### `models.GcpCredentialsServiceMetadata`

```typescript
const value: models.GcpCredentialsServiceMetadata = {
  type: "serviceMetadata",
};
```

### `models.GcpCredentialsProjectedServiceAccount`

```typescript
const value: models.GcpCredentialsProjectedServiceAccount = {
  serviceAccountEmail: "<value>",
  tokenFile: "<value>",
  type: "projectedServiceAccount",
};
```

### `models.GcpCredentialsExternalAccount`

```typescript
const value: models.GcpCredentialsExternalAccount = {
  audience: "<value>",
  credentialSourceFile: "<value>",
  subjectTokenType: "<value>",
  tokenUrl: "https://everlasting-platter.net",
  type: "externalAccount",
};
```

### `models.GcpCredentialsAuthorizedUser`

```typescript
const value: models.GcpCredentialsAuthorizedUser = {
  clientId: "<id>",
  clientSecret: "<value>",
  refreshToken: "<value>",
  type: "authorizedUser",
};
```

