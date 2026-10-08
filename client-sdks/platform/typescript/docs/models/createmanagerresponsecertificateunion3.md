# CreateManagerResponseCertificateUnion3

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.CreateManagerResponseCertificateTLSSecretRef3`

```typescript
const value: models.CreateManagerResponseCertificateTLSSecretRef3 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.CreateManagerResponseCertificateManagedAcmImport3`

```typescript
const value: models.CreateManagerResponseCertificateManagedAcmImport3 = {
  mode: "managedAcmImport",
};
```

### `models.CreateManagerResponseCertificateAwsAcmArn3`

```typescript
const value: models.CreateManagerResponseCertificateAwsAcmArn3 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.CreateManagerResponseCertificateManagedTLSSecret3`

```typescript
const value: models.CreateManagerResponseCertificateManagedTLSSecret3 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.CreateManagerResponseCertificateNone3`

```typescript
const value: models.CreateManagerResponseCertificateNone3 = {
  mode: "none",
};
```

