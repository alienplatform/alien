# ManagerRetryResponseCertificateUnion6

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.ManagerRetryResponseCertificateTLSSecretRef6`

```typescript
const value: models.ManagerRetryResponseCertificateTLSSecretRef6 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.ManagerRetryResponseCertificateManagedAcmImport6`

```typescript
const value: models.ManagerRetryResponseCertificateManagedAcmImport6 = {
  mode: "managedAcmImport",
};
```

### `models.ManagerRetryResponseCertificateAwsAcmArn6`

```typescript
const value: models.ManagerRetryResponseCertificateAwsAcmArn6 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.ManagerRetryResponseCertificateManagedTLSSecret6`

```typescript
const value: models.ManagerRetryResponseCertificateManagedTLSSecret6 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.ManagerRetryResponseCertificateNone6`

```typescript
const value: models.ManagerRetryResponseCertificateNone6 = {
  mode: "none",
};
```

