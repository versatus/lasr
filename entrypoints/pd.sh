#! /bin/sh
set -e

if [ $SLOT = 1 ]; then 
    exec ./pd-server \
        --name $NAME \
        --client-urls http://0.0.0.0:2379 \
        --peer-urls http://0.0.0.0:2380 \
        --advertise-client-urls http://`cat /etc/hostname`:2379 \
        --advertise-peer-urls http://`cat /etc/hostname`:2380
else
    exec ./pd-server \
        --name $NAME \
        --client-urls http://0.0.0.0:2379 \
        --peer-urls http://0.0.0.0:2380 \
        --advertise-client-urls http://`cat /etc/hostname`:2379 \
        --advertise-peer-urls http://`cat /etc/hostname`:2380 \
        --join http://pd.tikv:2379
fi