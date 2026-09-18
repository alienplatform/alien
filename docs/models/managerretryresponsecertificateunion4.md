# ManagerRetryResponseCertificateUnion4

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.ManagerRetryResponseCertificateTLSSecretRef4`

```typescript
const value: models.ManagerRetryResponseCertificateTLSSecretRef4 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.ManagerRetryResponseCertificateManagedAcmImport4`

```typescript
const value: models.ManagerRetryResponseCertificateManagedAcmImport4 = {
  mode: "managedAcmImport",
};
```

### `models.ManagerRetryResponseCertificateAwsAcmArn4`

```typescript
const value: models.ManagerRetryResponseCertificateAwsAcmArn4 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.ManagerRetryResponseCertificateManagedTLSSecret4`

```typescript
const value: models.ManagerRetryResponseCertificateManagedTLSSecret4 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.ManagerRetryResponseCertificateNone4`

```typescript
const value: models.ManagerRetryResponseCertificateNone4 = {
  mode: "none",
};
```

