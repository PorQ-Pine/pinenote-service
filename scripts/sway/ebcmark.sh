#!/bin/sh

usage() {
    echo "usage: $0 [set <mark> [silent]|unset]"
    exit 1
}

get_ebchint_mark() {
    swaymsg -t get_tree | jq -c '..
        | try select(.focused == true)
        | [ .marks[] | capture("(?<m>.?ebchint:(?<k>[^:]+):.*)") ]
        | try first // empty'
}

unset_mark() {
    mark=$(echo $1 | jq -cr '.m')

    if [ -n "$mark" ]; then
        echo Removing $mark
        swaymsg -- unmark $mark 
    fi
}

set_mark() {
    ebcmark="$1" 
    new_hint="$2"
    mark="ebchint"

    if [ -z "$new_hint" ]; then
        usage
    fi

    id=$(echo $ebcmark | jq -cr '.k')

    if [ -z "$id" ]; then
        id=$(tr -d -c "A-Za-z0-9" < /dev/urandom | head -c 8)
    fi

    if [ -n "$3" ]; then
        mark='_ebchint' 
    fi

    echo "Adding mark $mark:$id:$new_hint"
    swaymsg -- mark --add "$mark:$id:$new_hint"
}

ebcmark=$(get_ebchint_mark)

action=$1
shift;
case $action in
    set)
        if [ -z $1 ]; then
            usage
        fi

        unset_mark "$ebcmark"
        set_mark "$ebcmark" "$@"
        ;;
    unset)
        unset_mark "$ebcmark"
        ;;
    *)
        usage
        ;;
esac
