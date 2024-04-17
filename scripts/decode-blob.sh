#!/bin/bash

BLOB=$1

echo $BLOB | q -r .data | kzgpad -d -
