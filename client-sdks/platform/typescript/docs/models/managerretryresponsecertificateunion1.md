# ManagerRetryResponseCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.ManagerRetryResponseCertificateTLSSecretRef1`

```typescript
const value: models.ManagerRetryResponseCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.ManagerRetryResponseCertificateManagedAcmImport1`

```typescript
const value: models.ManagerRetryResponseCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `models.ManagerRetryResponseCertificateAwsAcmArn1`

```typescript
const value: models.ManagerRetryResponseCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.ManagerRetryResponseCertificateManagedTLSSecret1`

```typescript
const value: models.ManagerRetryResponseCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.ManagerRetryResponseCertificateNone1`

```typescript
const value: models.ManagerRetryResponseCertificateNone1 = {
  mode: "none",
};
```

