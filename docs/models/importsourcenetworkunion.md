# ImportSourceNetworkUnion


## Supported Types

### `models.ImportSourceNetworkUseDefault`

```typescript
const value: models.ImportSourceNetworkUseDefault = {
  type: "use-default",
};
```

### `models.ImportSourceNetworkCreate`

```typescript
const value: models.ImportSourceNetworkCreate = {
  type: "create",
};
```

### `models.ImportSourceNetworkByoVpcAws`

```typescript
const value: models.ImportSourceNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
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

### `models.ImportSourceNetworkByoVpcGcp`

```typescript
const value: models.ImportSourceNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.ImportSourceNetworkByoVnetAzure`

```typescript
const value: models.ImportSourceNetworkByoVnetAzure = {
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

