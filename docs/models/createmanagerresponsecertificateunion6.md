# CreateManagerResponseCertificateUnion6

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.CreateManagerResponseCertificateTLSSecretRef6`

```typescript
const value: models.CreateManagerResponseCertificateTLSSecretRef6 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.CreateManagerResponseCertificateManagedAcmImport6`

```typescript
const value: models.CreateManagerResponseCertificateManagedAcmImport6 = {
  mode: "managedAcmImport",
};
```

### `models.CreateManagerResponseCertificateAwsAcmArn6`

```typescript
const value: models.CreateManagerResponseCertificateAwsAcmArn6 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.CreateManagerResponseCertificateManagedTLSSecret6`

```typescript
const value: models.CreateManagerResponseCertificateManagedTLSSecret6 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.CreateManagerResponseCertificateNone6`

```typescript
const value: models.CreateManagerResponseCertificateNone6 = {
  mode: "none",
};
```

