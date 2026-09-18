# SyncListResponseNetworkUnion


## Supported Types

### `models.SyncListResponseNetworkUseDefault`

```typescript
const value: models.SyncListResponseNetworkUseDefault = {
  type: "use-default",
};
```

### `models.SyncListResponseNetworkCreate`

```typescript
const value: models.SyncListResponseNetworkCreate = {
  type: "create",
};
```

### `models.SyncListResponseNetworkByoVpcAws`

```typescript
const value: models.SyncListResponseNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.SyncListResponseNetworkByoVpcGcp`

```typescript
const value: models.SyncListResponseNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.SyncListResponseNetworkByoVnetAzure`

```typescript
const value: models.SyncListResponseNetworkByoVnetAzure = {
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

