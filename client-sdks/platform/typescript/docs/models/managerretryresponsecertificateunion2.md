# ManagerRetryResponseCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.ManagerRetryResponseCertificateTLSSecretRef2`

```typescript
const value: models.ManagerRetryResponseCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.ManagerRetryResponseCertificateManagedAcmImport2`

```typescript
const value: models.ManagerRetryResponseCertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

### `models.ManagerRetryResponseCertificateAwsAcmArn2`

```typescript
const value: models.ManagerRetryResponseCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.ManagerRetryResponseCertificateManagedTLSSecret2`

```typescript
const value: models.ManagerRetryResponseCertificateManagedTLSSecret2 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.ManagerRetryResponseCertificateNone2`

```typescript
const value: models.ManagerRetryResponseCertificateNone2 = {
  mode: "none",
};
```

