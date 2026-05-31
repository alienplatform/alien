# SyncAcquireResponseCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.SyncAcquireResponseCertificateTLSSecretRef1`

```typescript
const value: models.SyncAcquireResponseCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.SyncAcquireResponseCertificateManagedAcmImport1`

```typescript
const value: models.SyncAcquireResponseCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `models.SyncAcquireResponseCertificateAwsAcmArn1`

```typescript
const value: models.SyncAcquireResponseCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.SyncAcquireResponseCertificateManagedTLSSecret1`

```typescript
const value: models.SyncAcquireResponseCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.SyncAcquireResponseCertificateNone1`

```typescript
const value: models.SyncAcquireResponseCertificateNone1 = {
  mode: "none",
};
```

