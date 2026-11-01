package com.gathin.touchiptv.ui;

import android.content.Context;
import android.view.LayoutInflater;
import android.view.View;
import android.view.ViewGroup;
import android.widget.BaseAdapter;
import android.widget.ImageView;
import android.widget.TextView;

import com.gathin.touchiptv.R;
import com.gathin.touchiptv.model.Channel;

import java.util.ArrayList;
import java.util.List;

public class ChannelAdapter extends BaseAdapter {

    private final LayoutInflater inflater;
    private final Context ctx;
    private final List<Channel> items = new ArrayList<Channel>();
    private String currentName = "";

    public ChannelAdapter(Context ctx) {
        this.ctx = ctx;
        this.inflater = LayoutInflater.from(ctx);
    }

    public void setItems(List<Channel> list) {
        items.clear();
        if (list != null) {
            items.addAll(list);
        }
        notifyDataSetChanged();
    }

    public List<Channel> getItems() {
        return items;
    }

    public void setCurrent(String name) {
        currentName = name == null ? "" : name;
        notifyDataSetChanged();
    }

    public int indexOf(String name) {
        for (int i = 0; i < items.size(); i++) {
            if (items.get(i).name.equals(name)) {
                return i;
            }
        }
        return -1;
    }

    @Override
    public int getCount() {
        return items.size();
    }

    @Override
    public Channel getItem(int position) {
        return items.get(position);
    }

    @Override
    public long getItemId(int position) {
        return position;
    }

    @Override
    public View getView(int position, View convertView, ViewGroup parent) {
        View row = convertView;
        if (row == null) {
            row = inflater.inflate(R.layout.item_channel, parent, false);
            row.setTag(new Holder(row));
        }
        Holder h = (Holder) row.getTag();
        Channel c = items.get(position);

        h.index.setText(String.format("%02d", position + 1));
        h.name.setText(c.name);

        boolean isCurrent = c.name.equals(currentName);
        h.bar.setVisibility(isCurrent ? View.VISIBLE : View.INVISIBLE);
        h.name.setTextColor(ctx.getResources().getColor(isCurrent ? R.color.accent : R.color.text_primary));
        row.setActivated(isCurrent);

        if (c.resolution != null && c.resolution.length() > 0) {
            h.res.setVisibility(View.VISIBLE);
            h.res.setText(c.resolution);
        } else {
            h.res.setVisibility(View.GONE);
        }

        if (c.latencyKnown) {
            h.latency.setVisibility(View.VISIBLE);
            h.latency.setText(Ui.latencyText(c.latency));
            h.latency.setTextColor(Ui.latencyColor(ctx, c.latency));
        } else {
            h.latency.setVisibility(View.GONE);
        }

        h.star.setVisibility(c.favorite ? View.VISIBLE : View.INVISIBLE);
        Ui.tint(h.star, ctx.getResources().getColor(R.color.amber));

        return row;
    }

    private static final class Holder {
        final View bar;
        final TextView index;
        final TextView name;
        final TextView res;
        final TextView latency;
        final ImageView star;

        Holder(View row) {
            bar = row.findViewById(R.id.row_bar);
            index = (TextView) row.findViewById(R.id.row_idx);
            name = (TextView) row.findViewById(R.id.row_name);
            res = (TextView) row.findViewById(R.id.row_res);
            latency = (TextView) row.findViewById(R.id.row_latency);
            star = (ImageView) row.findViewById(R.id.row_star);
        }
    }
}
