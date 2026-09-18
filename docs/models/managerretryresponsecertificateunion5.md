# ManagerRetryResponseCertificateUnion5

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.ManagerRetryResponseCertificateTLSSecretRef5`

```typescript
const value: models.ManagerRetryResponseCertificateTLSSecretRef5 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.ManagerRetryResponseCertificateManagedAcmImport5`

```typescript
const value: models.ManagerRetryResponseCertificateManagedAcmImport5 = {
  mode: "managedAcmImport",
};
```

### `models.ManagerRetryResponseCertificateAwsAcmArn5`

```typescript
const value: models.ManagerRetryResponseCertificateAwsAcmArn5 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.ManagerRetryResponseCertificateManagedTLSSecret5`

```typescript
const value: models.ManagerRetryResponseCertificateManagedTLSSecret5 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.ManagerRetryResponseCertificateNone5`

```typescript
const value: models.ManagerRetryResponseCertificateNone5 = {
  mode: "none",
};
```

