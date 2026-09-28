package com.nl2sh.bridge;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.net.Uri;
import android.os.Binder;
import android.os.Bundle;
import android.util.Base64;
import java.nio.charset.StandardCharsets;
import org.json.JSONException;
import org.json.JSONObject;

/** Binder entry point restricted to Android shell/root callers. */
public final class BridgeProvider extends ContentProvider {
    private static final int MAX_REPLY = 64 * 1024;

    @Override
    public boolean onCreate() {
        return true;
    }

    @Override
    public Bundle call(String method, String arg, Bundle extras) {
        int uid = Binder.getCallingUid();
        if (uid != 0 && uid != 2000) {
            throw new SecurityException("nl2sh bridge requires shell or root UID");
        }
        JSONObject request = new JSONObject();
        JSONObject reply;
        try {
            request.put("action", method);
            if (extras != null) {
                request.put("text", extras.getString("text", ""));
                request.put("package", extras.getString("package", ""));
                request.put("bounds", extras.getString("bounds", ""));
                request.put("class", extras.getString("class", ""));
                request.put("resource_id", extras.getString("resource_id", ""));
                request.put("node_text", extras.getString("node_text", ""));
                request.put("description", extras.getString("description", ""));
                request.put("text_hash", extras.getString("text_hash", ""));
                request.put("description_hash", extras.getString("description_hash", ""));
                for (String coordinate : new String[] {"x", "y", "end_x", "end_y", "duration_ms"}) {
                    request.put(coordinate, extras.getString(coordinate, ""));
                }
            }
            reply = BridgeService.dispatch(request);
        } catch (JSONException failure) {
            reply = BridgeService.error("invalid request");
        }
        byte[] encoded = reply.toString().getBytes(StandardCharsets.UTF_8);
        if (encoded.length > MAX_REPLY) {
            encoded = BridgeService.error("reply exceeds size limit")
                    .toString().getBytes(StandardCharsets.UTF_8);
        }
        Bundle result = new Bundle();
        result.putString("data", Base64.encodeToString(encoded,
                Base64.URL_SAFE | Base64.NO_PADDING | Base64.NO_WRAP));
        return result;
    }

    @Override
    public Cursor query(Uri uri, String[] projection, String selection,
            String[] selectionArgs, String sortOrder) {
        throw new UnsupportedOperationException();
    }

    @Override
    public String getType(Uri uri) {
        throw new UnsupportedOperationException();
    }

    @Override
    public Uri insert(Uri uri, ContentValues values) {
        throw new UnsupportedOperationException();
    }

    @Override
    public int delete(Uri uri, String selection, String[] selectionArgs) {
        throw new UnsupportedOperationException();
    }

    @Override
    public int update(Uri uri, ContentValues values, String selection,
            String[] selectionArgs) {
        throw new UnsupportedOperationException();
    }
}
