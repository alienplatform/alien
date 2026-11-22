# CreateManagerResponseCertificateUnion5

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.CreateManagerResponseCertificateTLSSecretRef5`

```typescript
const value: models.CreateManagerResponseCertificateTLSSecretRef5 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.CreateManagerResponseCertificateManagedAcmImport5`

```typescript
const value: models.CreateManagerResponseCertificateManagedAcmImport5 = {
  mode: "managedAcmImport",
};
```

### `models.CreateManagerResponseCertificateAwsAcmArn5`

```typescript
const value: models.CreateManagerResponseCertificateAwsAcmArn5 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.CreateManagerResponseCertificateManagedTLSSecret5`

```typescript
const value: models.CreateManagerResponseCertificateManagedTLSSecret5 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.CreateManagerResponseCertificateNone5`

```typescript
const value: models.CreateManagerResponseCertificateNone5 = {
  mode: "none",
};
```

