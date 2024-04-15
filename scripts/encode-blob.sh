#!/bin/bash

DATA=$1
export PATH=/usr/local/bin/kzgpad:$PATH
kzgpad -e $DATA
