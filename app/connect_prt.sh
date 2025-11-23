APP_NAME="com.skygrel19.vulkan_lib"

# Get current timestamp from the Android device
TIMESTAMP=$(adb shell logcat -v time -t 1 | grep -v 'beginning of' | head -1 | awk '{print $1, $2}')

# Find the PID of the app by its package name using the 'ps' command
PID=$(adb shell ps | grep $APP_NAME | awk '{print $2}')

if [ -z "$PID" ]; then
    echo "PID not found for app: $APP_NAME"
    exit 1
fi

echo "Starting logcat..."
echo "Filters: Timestamp: $TIMESTAMP, PID: $PID"

# Launch logcat with the required filters
adb logcat -v color --pid=$PID -T "$TIMESTAMP"