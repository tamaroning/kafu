resources:
- ../../bases

{{ if instance_id }}namePrefix: {instance_id}-
{{ endif }}nameSuffix: -{node_id}

patches:
- path: patch.yaml
