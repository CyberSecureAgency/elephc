<?php
// The ftp:// wrapper opens a file on an FTP server as a readable stream.
// fopen() connects, logs in anonymously, and retrieves the file in binary
// passive mode. Running this example requires outbound network access.

$handle = fopen("ftp://ftp.gnu.org/README", "r");
if ($handle === false) {
    echo "could not open the ftp:// stream (network access required)\n";
} else {
    $contents = fread($handle, 256);
    fclose($handle);
    echo "fetched " . strlen($contents) . " bytes over ftp://\n";
}
