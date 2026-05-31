# ManagerRetryResponseCertificateUnion3

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.ManagerRetryResponseCertificateTLSSecretRef3`

```typescript
const value: models.ManagerRetryResponseCertificateTLSSecretRef3 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.ManagerRetryResponseCertificateManagedAcmImport3`

```typescript
const value: models.ManagerRetryResponseCertificateManagedAcmImport3 = {
  mode: "managedAcmImport",
};
```

### `models.ManagerRetryResponseCertificateAwsAcmArn3`

```typescript
const value: models.ManagerRetryResponseCertificateAwsAcmArn3 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.ManagerRetryResponseCertificateManagedTLSSecret3`

```typescript
const value: models.ManagerRetryResponseCertificateManagedTLSSecret3 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.ManagerRetryResponseCertificateNone3`

```typescript
const value: models.ManagerRetryResponseCertificateNone3 = {
  mode: "none",
};
```

