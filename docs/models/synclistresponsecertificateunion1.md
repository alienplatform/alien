# SyncListResponseCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.SyncListResponseCertificateTLSSecretRef1`

```typescript
const value: models.SyncListResponseCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.SyncListResponseCertificateManagedAcmImport1`

```typescript
const value: models.SyncListResponseCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `models.SyncListResponseCertificateAwsAcmArn1`

```typescript
const value: models.SyncListResponseCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.SyncListResponseCertificateManagedTLSSecret1`

```typescript
const value: models.SyncListResponseCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.SyncListResponseCertificateNone1`

```typescript
const value: models.SyncListResponseCertificateNone1 = {
  mode: "none",
};
```

