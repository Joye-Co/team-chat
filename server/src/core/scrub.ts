// Removes the content of a deleted message from a group room's SQLite: the message row, its
// stored events, and the quote excerpts of messages that quote it. Used for live deletes and for
// `message.deleted` events imported from the archive. Attachments and 产出 references are removed
// by the files feature (its event writer for `message.deleted`).

export function scrubDeletedMessage(sql: SqlStorage, messageId: string, deletedAt: number): void {
  sql.exec(
    `UPDATE messages SET body = '', deleted_at = ?, streaming = 0,
       ext = json_remove(ext, '$.card', '$.cardV2', '$.replyTo', '$.reactions') WHERE id = ?`,
    deletedAt,
    messageId,
  );
  sql.exec(
    `UPDATE events SET data = json_remove(json_set(data, '$.message.body', ''),
       '$.message.card', '$.message.cardV2', '$.message.replyTo', '$.message.reactions')
     WHERE message_id = ? AND type = 'message.created'`,
    messageId,
  );
  sql.exec(
    "UPDATE events SET data = json_remove(json_set(data, '$.body', ''), '$.card', '$.cardV2') WHERE message_id = ? AND type = 'message.updated'",
    messageId,
  );
  // Answers to a deleted card lose what the answerer typed (1.1).
  sql.exec(
    "UPDATE events SET data = json_remove(data, '$.inputs', '$.editedValues') WHERE message_id = ? AND type = 'card.answered'",
    messageId,
  );
  // Quotes of the deleted message keep the link but lose its text.
  const quoting = sql
    .exec<{ id: string }>("SELECT id FROM messages WHERE json_extract(ext, '$.replyTo.messageId') = ?", messageId)
    .toArray()
    .map((r) => r.id);
  for (const id of quoting) {
    sql.exec("UPDATE messages SET ext = json_set(ext, '$.replyTo.excerpt', '') WHERE id = ?", id);
    sql.exec(
      `UPDATE events SET data = json_set(data, '$.message.replyTo.excerpt', '')
       WHERE message_id = ? AND type = 'message.created' AND json_type(data, '$.message.replyTo') = 'object'`,
      id,
    );
  }
}
