<?php

// Userspace stream wrapper that intercepts filesystem-metadata changes.
//
// When a path uses a registered scheme://, PHP routes chmod()/chown()/chgrp()
// to the wrapper's stream_metadata($path, $option, $value) method instead of
// the real filesystem. The $option tells the wrapper which change is requested:
//   STREAM_META_ACCESS (6) -> chmod, value = mode
//   STREAM_META_OWNER  (3) -> chown with an integer uid, value = uid
//   STREAM_META_GROUP  (5) -> chgrp with an integer gid, value = gid

class MetaFs
{
    public $context;

    public function stream_metadata(string $path, int $option, $value): bool
    {
        $label = "meta";
        if ($option === 6) {
            $label = "chmod";
        } elseif ($option === 3) {
            $label = "chown";
        } elseif ($option === 5) {
            $label = "chgrp";
        }
        echo "$label $path option=$option value=$value\n";
        return true;
    }
}

stream_wrapper_register("meta", "MetaFs");

chmod("meta://app.log", 0644);
chown("meta://app.log", 1000);
chgrp("meta://app.log", 2000);
