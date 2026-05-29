# CreateManagerResponseCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.CreateManagerResponseCertificateTLSSecretRef1`

```typescript
const value: models.CreateManagerResponseCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.CreateManagerResponseCertificateManagedAcmImport1`

```typescript
const value: models.CreateManagerResponseCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `models.CreateManagerResponseCertificateAwsAcmArn1`

```typescript
const value: models.CreateManagerResponseCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.CreateManagerResponseCertificateManagedTLSSecret1`

```typescript
const value: models.CreateManagerResponseCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.CreateManagerResponseCertificateNone1`

```typescript
const value: models.CreateManagerResponseCertificateNone1 = {
  mode: "none",
};
```

