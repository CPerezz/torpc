#!/bin/bash
# Start Tor with our configuration

echo "Starting Tor hidden service..."
tor -f configs/torrc
