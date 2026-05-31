# CreateManagerResponseCertificateUnion4

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.CreateManagerResponseCertificateTLSSecretRef4`

```typescript
const value: models.CreateManagerResponseCertificateTLSSecretRef4 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.CreateManagerResponseCertificateManagedAcmImport4`

```typescript
const value: models.CreateManagerResponseCertificateManagedAcmImport4 = {
  mode: "managedAcmImport",
};
```

### `models.CreateManagerResponseCertificateAwsAcmArn4`

```typescript
const value: models.CreateManagerResponseCertificateAwsAcmArn4 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.CreateManagerResponseCertificateManagedTLSSecret4`

```typescript
const value: models.CreateManagerResponseCertificateManagedTLSSecret4 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.CreateManagerResponseCertificateNone4`

```typescript
const value: models.CreateManagerResponseCertificateNone4 = {
  mode: "none",
};
```

