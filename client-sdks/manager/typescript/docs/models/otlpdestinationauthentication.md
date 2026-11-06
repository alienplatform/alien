# OtlpDestinationAuthentication


## Supported Types

### `models.OtlpDestinationAuthenticationBearer`

```typescript
const value: models.OtlpDestinationAuthenticationBearer = {
  tokenEnvironmentVariable: "<value>",
  type: "bearer",
};
```

### `models.OtlpDestinationAuthenticationJwtHs256`

```typescript
const value: models.OtlpDestinationAuthenticationJwtHs256 = {
  audience: "<value>",
  claims: {
    "key": "<value>",
  },
  issuer: "jcb",
  keyEnvironmentVariable: "<value>",
  type: "jwtHs256",
};
```
