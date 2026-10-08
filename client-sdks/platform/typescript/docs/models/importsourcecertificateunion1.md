# ImportSourceCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.ImportSourceCertificateTLSSecretRef1`

```typescript
const value: models.ImportSourceCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.ImportSourceCertificateManagedAcmImport1`

```typescript
const value: models.ImportSourceCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `models.ImportSourceCertificateAwsAcmArn1`

```typescript
const value: models.ImportSourceCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.ImportSourceCertificateManagedTLSSecret1`

```typescript
const value: models.ImportSourceCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.ImportSourceCertificateNone1`

```typescript
const value: models.ImportSourceCertificateNone1 = {
  mode: "none",
};
```

