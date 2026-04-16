# SyncAcquireResponseNetworkUnion


## Supported Types

### `models.SyncAcquireResponseNetworkUseDefault`

```typescript
const value: models.SyncAcquireResponseNetworkUseDefault = {
  type: "use-default",
};
```

### `models.SyncAcquireResponseNetworkCreate`

```typescript
const value: models.SyncAcquireResponseNetworkCreate = {
  type: "create",
};
```

### `models.SyncAcquireResponseNetworkByoVpcAws`

```typescript
const value: models.SyncAcquireResponseNetworkByoVpcAws = {
  privateSubnetIds: [],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.SyncAcquireResponseNetworkByoVpcGcp`

```typescript
const value: models.SyncAcquireResponseNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.SyncAcquireResponseNetworkByoVnetAzure`

```typescript
const value: models.SyncAcquireResponseNetworkByoVnetAzure = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

### `any`

```typescript
const value: any = "<value>";
```

