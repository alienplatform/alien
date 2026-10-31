def remove_setup_notification_rules(state, selected):
    """Remove the S3 queue entries this setup wrote before detaching their bucket notification.

    Detaching keeps the bucket's notification document, and teardown then deletes the queues
    it delivers to. A refresh records every entry of the document in state, including ones
    added by hand, so only Ids this package generates (<prefix><marker><n>, the marker
    carrying the Storage's hash) are removed. S3 checks every destination left in the
    document; if one no longer exists, the write fails here and nothing is detached.
    """
    addresses = {item['address'] for item in selected}
    for resource in state.get('resources', []):
        if resource.get('mode') != 'managed' or resource.get('type') != 'aws_s3_bucket_notification':
            continue
        base = '.'.join(filter(None, [resource.get('module'), resource['type'], resource['name']]))
        for instance in resource.get('instances', []):
            suffix = '[' + json.dumps(instance['index_key']) + ']' if 'index_key' in instance else ''
            if base + suffix not in addresses:
                continue
            attributes = instance.get('attributes', {})
            bucket = attributes.get('bucket')
            marker = SETUP_NOTIFICATION_ID_MARKERS.get(resource['type'] + '.' + resource['name'])
            if not bucket or not marker:
                raise RuntimeError('Refresh the bucket notification state of ' + base + suffix + ' before retention; nothing detached')
            owned = re.compile('.+' + re.escape(marker) + '[0-9]+')
            region = ['--region', attributes['region']] if attributes.get('region') else []
            def s3api(operation, *arguments):
                try:
                    return subprocess.check_output(
                        ['aws', 's3api', operation, *region, '--bucket', bucket, *arguments,
                         '--output', 'json', '--no-cli-pager'], text=True)
                except (FileNotFoundError, subprocess.CalledProcessError) as error:
                    raise RuntimeError('Removing this setup\'s notifications from ' + bucket + ' requires AWS CLI with the installer credentials and s3:GetBucketNotification/s3:PutBucketNotification. S3 checks every destination left in the document: fix or remove an entry whose queue, topic or function no longer exists, then rerun; nothing detached') from error
            document = json.loads(s3api('get-bucket-notification-configuration') or '{}')
            document.pop('ResponseMetadata', None)
            queues = document.get('QueueConfigurations', [])
            kept = [entry for entry in queues if not owned.fullmatch(entry.get('Id') or '')]
            if len(kept) == len(queues):
                continue
            if kept:
                document['QueueConfigurations'] = kept
            else:
                document.pop('QueueConfigurations')
            s3api('put-bucket-notification-configuration', '--notification-configuration', json.dumps(document))


